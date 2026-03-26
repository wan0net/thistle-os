// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Structured data logger
//
// Provides structured data logging for apps to record timestamped sensor
// readings in CSV format. Data is managed in memory; apps export via to_csv()
// and write to files through existing filesystem syscalls.

use std::os::raw::c_char;

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_INVALID_ARG: i32 = 0x102;

// ── Data types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnType {
    Int,
    Float,
    Text,
    Bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DataValue {
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone)]
pub struct DataColumn {
    pub name: String,
    pub col_type: ColumnType,
}

#[derive(Debug, Clone)]
pub struct DataRow {
    pub timestamp: u32,
    pub values: Vec<DataValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnStats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub count: usize,
}

// ── Timestamp formatting ────────────────────────────────────────────

/// Convert a Unix timestamp (seconds since 1970-01-01) to ISO 8601 string.
/// Uses Howard Hinnant's civil_from_days algorithm.
fn unix_to_iso8601(ts: u32) -> String {
    let secs = ts as u64;
    let days_since_epoch = (secs / 86400) as i64;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Howard Hinnant's civil_from_days algorithm
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month offset [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

// ── CSV helpers ─────────────────────────────────────────────────────

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn data_value_to_csv(v: &DataValue) -> String {
    match v {
        DataValue::Int(n) => n.to_string(),
        DataValue::Float(f) => format!("{:.6}", f),
        DataValue::Text(s) => csv_escape(s),
        DataValue::Bool(b) => if *b { "true".to_string() } else { "false".to_string() },
        DataValue::Null => String::new(),
    }
}

// ── DataLogger ──────────────────────────────────────────────────────

pub struct DataLogger {
    name: String,
    columns: Vec<DataColumn>,
    rows: Vec<DataRow>,
    pending_row: Option<(u32, Vec<DataValue>)>,
}

impl DataLogger {
    pub fn new(name: &str) -> DataLogger {
        DataLogger {
            name: name.to_string(),
            columns: Vec::new(),
            rows: Vec::new(),
            pending_row: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn add_column(&mut self, name: &str, col_type: ColumnType) -> Result<usize, i32> {
        if !self.rows.is_empty() {
            return Err(ESP_FAIL);
        }
        let index = self.columns.len();
        self.columns.push(DataColumn {
            name: name.to_string(),
            col_type,
        });
        Ok(index)
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn log_row(&mut self, timestamp: u32, values: Vec<DataValue>) -> Result<(), i32> {
        if values.len() != self.columns.len() {
            return Err(ESP_ERR_INVALID_ARG);
        }
        // Type-check each value
        for (i, v) in values.iter().enumerate() {
            match (&self.columns[i].col_type, v) {
                (_, DataValue::Null) => {} // Null is always allowed
                (ColumnType::Int, DataValue::Int(_)) => {}
                (ColumnType::Float, DataValue::Float(_)) => {}
                (ColumnType::Text, DataValue::Text(_)) => {}
                (ColumnType::Bool, DataValue::Bool(_)) => {}
                _ => return Err(ESP_ERR_INVALID_ARG),
            }
        }
        self.rows.push(DataRow { timestamp, values });
        Ok(())
    }

    pub fn get_row(&self, index: usize) -> Option<&DataRow> {
        self.rows.get(index)
    }

    pub fn get_column_values(&self, col_index: usize) -> Vec<&DataValue> {
        self.rows.iter().map(|r| &r.values[col_index]).collect()
    }

    pub fn clear_rows(&mut self) {
        self.rows.clear();
    }

    pub fn time_range(&self) -> Option<(u32, u32)> {
        if self.rows.is_empty() {
            return None;
        }
        let first = self.rows.first().unwrap().timestamp;
        let last = self.rows.last().unwrap().timestamp;
        Some((first, last))
    }

    pub fn stats(&self, col_index: usize) -> Option<ColumnStats> {
        if col_index >= self.columns.len() {
            return None;
        }
        match self.columns[col_index].col_type {
            ColumnType::Text | ColumnType::Bool => return None,
            _ => {}
        }

        let mut min = f64::MAX;
        let mut max = f64::MIN;
        let mut sum = 0.0_f64;
        let mut count = 0_usize;

        for row in &self.rows {
            let val = match &row.values[col_index] {
                DataValue::Int(n) => *n as f64,
                DataValue::Float(f) => *f,
                DataValue::Null => continue,
                _ => continue,
            };
            if val < min {
                min = val;
            }
            if val > max {
                max = val;
            }
            sum += val;
            count += 1;
        }

        if count == 0 {
            return Some(ColumnStats {
                min: 0.0,
                max: 0.0,
                mean: 0.0,
                count: 0,
            });
        }

        Some(ColumnStats {
            min,
            max,
            mean: sum / count as f64,
            count,
        })
    }

    pub fn to_csv(&self) -> String {
        let mut out = String::new();

        // Header row: timestamp + column names
        out.push_str("timestamp");
        for col in &self.columns {
            out.push(',');
            out.push_str(&csv_escape(&col.name));
        }
        out.push('\n');

        // Data rows
        for row in &self.rows {
            out.push_str(&unix_to_iso8601(row.timestamp));
            for v in &row.values {
                out.push(',');
                out.push_str(&data_value_to_csv(v));
            }
            out.push('\n');
        }

        out
    }

    // ── Pending row API (used by FFI) ───────────────────────────────

    fn begin_row(&mut self, timestamp: u32) -> Result<(), i32> {
        if self.pending_row.is_some() {
            return Err(ESP_FAIL);
        }
        let nulls = vec![DataValue::Null; self.columns.len()];
        self.pending_row = Some((timestamp, nulls));
        Ok(())
    }

    fn set_pending_value(&mut self, col: usize, value: DataValue) -> Result<(), i32> {
        let pending = match self.pending_row.as_mut() {
            Some(p) => p,
            None => return Err(ESP_FAIL),
        };
        if col >= self.columns.len() {
            return Err(ESP_ERR_INVALID_ARG);
        }
        // Type-check
        match (&self.columns[col].col_type, &value) {
            (_, DataValue::Null) => {}
            (ColumnType::Int, DataValue::Int(_)) => {}
            (ColumnType::Float, DataValue::Float(_)) => {}
            (ColumnType::Text, DataValue::Text(_)) => {}
            (ColumnType::Bool, DataValue::Bool(_)) => {}
            _ => return Err(ESP_ERR_INVALID_ARG),
        }
        pending.1[col] = value;
        Ok(())
    }

    fn commit_row(&mut self) -> Result<(), i32> {
        let (ts, values) = match self.pending_row.take() {
            Some(p) => p,
            None => return Err(ESP_FAIL),
        };
        self.rows.push(DataRow {
            timestamp: ts,
            values,
        });
        Ok(())
    }
}

// ── C FFI exports ───────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_create(name: *const c_char) -> *mut DataLogger {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    let c_str = std::ffi::CStr::from_ptr(name);
    let name_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    Box::into_raw(Box::new(DataLogger::new(name_str)))
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_destroy(logger: *mut DataLogger) {
    if !logger.is_null() {
        let _ = Box::from_raw(logger);
    }
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_add_column(
    logger: *mut DataLogger,
    name: *const c_char,
    col_type: i32,
) -> i32 {
    if logger.is_null() || name.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &mut *logger;
    let c_str = std::ffi::CStr::from_ptr(name);
    let name_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let ct = match col_type {
        0 => ColumnType::Int,
        1 => ColumnType::Float,
        2 => ColumnType::Text,
        3 => ColumnType::Bool,
        _ => return ESP_ERR_INVALID_ARG,
    };
    match logger.add_column(name_str, ct) {
        Ok(idx) => idx as i32,
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_begin_row(
    logger: *mut DataLogger,
    timestamp: u32,
) -> i32 {
    if logger.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &mut *logger;
    match logger.begin_row(timestamp) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_set_int(
    logger: *mut DataLogger,
    col: i32,
    value: i64,
) -> i32 {
    if logger.is_null() || col < 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &mut *logger;
    match logger.set_pending_value(col as usize, DataValue::Int(value)) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_set_float(
    logger: *mut DataLogger,
    col: i32,
    value: f64,
) -> i32 {
    if logger.is_null() || col < 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &mut *logger;
    match logger.set_pending_value(col as usize, DataValue::Float(value)) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_set_text(
    logger: *mut DataLogger,
    col: i32,
    text: *const c_char,
) -> i32 {
    if logger.is_null() || col < 0 || text.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &mut *logger;
    let c_str = std::ffi::CStr::from_ptr(text);
    let s = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    match logger.set_pending_value(col as usize, DataValue::Text(s.to_string())) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_set_bool(
    logger: *mut DataLogger,
    col: i32,
    value: i32,
) -> i32 {
    if logger.is_null() || col < 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &mut *logger;
    match logger.set_pending_value(col as usize, DataValue::Bool(value != 0)) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_commit_row(logger: *mut DataLogger) -> i32 {
    if logger.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &mut *logger;
    match logger.commit_row() {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_to_csv(
    logger: *const DataLogger,
    buf: *mut u8,
    buf_len: usize,
) -> i32 {
    if logger.is_null() || buf.is_null() || buf_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &*logger;
    let csv = logger.to_csv();
    let bytes = csv.as_bytes();
    if bytes.len() > buf_len {
        return ESP_FAIL;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
    bytes.len() as i32
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_row_count(logger: *const DataLogger) -> i32 {
    if logger.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &*logger;
    logger.row_count() as i32
}

#[no_mangle]
pub unsafe extern "C" fn rs_data_logger_column_count(logger: *const DataLogger) -> i32 {
    if logger.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let logger = &*logger;
    logger.column_count() as i32
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_create_logger_with_columns() {
        let mut logger = DataLogger::new("sensors");
        assert_eq!(logger.name(), "sensors");
        assert_eq!(logger.column_count(), 0);
        assert!(logger.is_empty());

        let idx0 = logger.add_column("temperature", ColumnType::Float).unwrap();
        let idx1 = logger.add_column("humidity", ColumnType::Int).unwrap();
        let idx2 = logger.add_column("location", ColumnType::Text).unwrap();

        assert_eq!(idx0, 0);
        assert_eq!(idx1, 1);
        assert_eq!(idx2, 2);
        assert_eq!(logger.column_count(), 3);
    }

    #[test]
    fn test_schema_locking() {
        let mut logger = DataLogger::new("test");
        logger.add_column("val", ColumnType::Int).unwrap();
        logger
            .log_row(1000, vec![DataValue::Int(42)])
            .unwrap();

        // Can't add columns after first row
        let result = logger.add_column("extra", ColumnType::Float);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ESP_FAIL);
    }

    #[test]
    fn test_log_rows_correct_types() {
        let mut logger = DataLogger::new("test");
        logger.add_column("temp", ColumnType::Float).unwrap();
        logger.add_column("count", ColumnType::Int).unwrap();
        logger.add_column("active", ColumnType::Bool).unwrap();

        let result = logger.log_row(
            1000,
            vec![
                DataValue::Float(23.5),
                DataValue::Int(10),
                DataValue::Bool(true),
            ],
        );
        assert!(result.is_ok());
        assert_eq!(logger.row_count(), 1);
        assert!(!logger.is_empty());
    }

    #[test]
    fn test_type_mismatch_error() {
        let mut logger = DataLogger::new("test");
        logger.add_column("temp", ColumnType::Float).unwrap();

        // Try to log an Int where Float is expected
        let result = logger.log_row(1000, vec![DataValue::Int(42)]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ESP_ERR_INVALID_ARG);
    }

    #[test]
    fn test_wrong_column_count_error() {
        let mut logger = DataLogger::new("test");
        logger.add_column("a", ColumnType::Int).unwrap();
        logger.add_column("b", ColumnType::Int).unwrap();

        // Too few values
        let result = logger.log_row(1000, vec![DataValue::Int(1)]);
        assert!(result.is_err());

        // Too many values
        let result = logger.log_row(
            1000,
            vec![DataValue::Int(1), DataValue::Int(2), DataValue::Int(3)],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_csv_export_with_header() {
        let mut logger = DataLogger::new("test");
        logger.add_column("value", ColumnType::Int).unwrap();
        logger
            .log_row(0, vec![DataValue::Int(42)])
            .unwrap();

        let csv = logger.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "timestamp,value");
        assert!(lines[1].starts_with("1970-01-01T00:00:00Z,42"));
    }

    #[test]
    fn test_csv_quoting_and_escaping() {
        let mut logger = DataLogger::new("test");
        logger.add_column("text", ColumnType::Text).unwrap();

        // Text with comma
        logger
            .log_row(0, vec![DataValue::Text("hello, world".to_string())])
            .unwrap();
        // Text with quote
        logger
            .log_row(1, vec![DataValue::Text("say \"hi\"".to_string())])
            .unwrap();
        // Text with newline
        logger
            .log_row(2, vec![DataValue::Text("line1\nline2".to_string())])
            .unwrap();

        let csv = logger.to_csv();
        let lines: Vec<&str> = csv.split('\n').collect();
        // Header
        assert_eq!(lines[0], "timestamp,text");
        // Comma in text -> quoted
        assert!(lines[1].contains("\"hello, world\""));
        // Quote in text -> doubled and quoted
        assert!(lines[2].contains("\"say \"\"hi\"\"\""));
        // Newline in text -> quoted (the field spans into the next "line")
        assert!(lines[3].contains("\"line1"));
    }

    #[test]
    fn test_csv_timestamp_formatting() {
        let mut logger = DataLogger::new("test");
        logger.add_column("v", ColumnType::Int).unwrap();

        // 2024-01-15 12:30:45 UTC = 1705321845
        logger
            .log_row(1705321845, vec![DataValue::Int(1)])
            .unwrap();

        let csv = logger.to_csv();
        assert!(csv.contains("2024-01-15T12:30:45Z"));
    }

    #[test]
    fn test_csv_float_precision() {
        let mut logger = DataLogger::new("test");
        logger.add_column("val", ColumnType::Float).unwrap();

        logger
            .log_row(0, vec![DataValue::Float(3.14159265)])
            .unwrap();

        let csv = logger.to_csv();
        assert!(csv.contains("3.141593")); // 6 decimal places
    }

    #[test]
    fn test_csv_null_handling() {
        let mut logger = DataLogger::new("test");
        logger.add_column("a", ColumnType::Int).unwrap();
        logger.add_column("b", ColumnType::Float).unwrap();

        logger
            .log_row(0, vec![DataValue::Null, DataValue::Null])
            .unwrap();

        let csv = logger.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        // Null values produce empty fields: "timestamp,,"
        assert!(lines[1].ends_with(",,"));
    }

    #[test]
    fn test_column_value_extraction() {
        let mut logger = DataLogger::new("test");
        logger.add_column("temp", ColumnType::Float).unwrap();
        logger.add_column("label", ColumnType::Text).unwrap();

        logger
            .log_row(0, vec![DataValue::Float(20.0), DataValue::Text("a".into())])
            .unwrap();
        logger
            .log_row(1, vec![DataValue::Float(25.0), DataValue::Text("b".into())])
            .unwrap();
        logger
            .log_row(2, vec![DataValue::Float(22.0), DataValue::Text("c".into())])
            .unwrap();

        let temps = logger.get_column_values(0);
        assert_eq!(temps.len(), 3);
        assert_eq!(*temps[0], DataValue::Float(20.0));
        assert_eq!(*temps[1], DataValue::Float(25.0));
        assert_eq!(*temps[2], DataValue::Float(22.0));

        let labels = logger.get_column_values(1);
        assert_eq!(*labels[0], DataValue::Text("a".into()));
    }

    #[test]
    fn test_stats_int_column() {
        let mut logger = DataLogger::new("test");
        logger.add_column("count", ColumnType::Int).unwrap();

        logger.log_row(0, vec![DataValue::Int(10)]).unwrap();
        logger.log_row(1, vec![DataValue::Int(20)]).unwrap();
        logger.log_row(2, vec![DataValue::Int(30)]).unwrap();

        let stats = logger.stats(0).unwrap();
        assert_eq!(stats.min, 10.0);
        assert_eq!(stats.max, 30.0);
        assert!((stats.mean - 20.0).abs() < f64::EPSILON);
        assert_eq!(stats.count, 3);
    }

    #[test]
    fn test_stats_float_column() {
        let mut logger = DataLogger::new("test");
        logger.add_column("temp", ColumnType::Float).unwrap();

        logger.log_row(0, vec![DataValue::Float(1.5)]).unwrap();
        logger.log_row(1, vec![DataValue::Float(2.5)]).unwrap();
        logger.log_row(2, vec![DataValue::Float(3.5)]).unwrap();

        let stats = logger.stats(0).unwrap();
        assert_eq!(stats.min, 1.5);
        assert_eq!(stats.max, 3.5);
        assert!((stats.mean - 2.5).abs() < f64::EPSILON);
        assert_eq!(stats.count, 3);
    }

    #[test]
    fn test_stats_returns_none_for_text() {
        let mut logger = DataLogger::new("test");
        logger.add_column("label", ColumnType::Text).unwrap();

        logger
            .log_row(0, vec![DataValue::Text("hello".into())])
            .unwrap();

        assert!(logger.stats(0).is_none());
    }

    #[test]
    fn test_stats_returns_none_for_bool() {
        let mut logger = DataLogger::new("test");
        logger.add_column("flag", ColumnType::Bool).unwrap();

        logger.log_row(0, vec![DataValue::Bool(true)]).unwrap();

        assert!(logger.stats(0).is_none());
    }

    #[test]
    fn test_time_range() {
        let mut logger = DataLogger::new("test");
        logger.add_column("v", ColumnType::Int).unwrap();

        assert!(logger.time_range().is_none());

        logger.log_row(100, vec![DataValue::Int(1)]).unwrap();
        logger.log_row(200, vec![DataValue::Int(2)]).unwrap();
        logger.log_row(300, vec![DataValue::Int(3)]).unwrap();

        let (first, last) = logger.time_range().unwrap();
        assert_eq!(first, 100);
        assert_eq!(last, 300);
    }

    #[test]
    fn test_clear_rows_keeps_schema() {
        let mut logger = DataLogger::new("test");
        logger.add_column("temp", ColumnType::Float).unwrap();
        logger.add_column("label", ColumnType::Text).unwrap();

        logger
            .log_row(0, vec![DataValue::Float(1.0), DataValue::Text("a".into())])
            .unwrap();
        assert_eq!(logger.row_count(), 1);

        logger.clear_rows();
        assert_eq!(logger.row_count(), 0);
        assert!(logger.is_empty());
        assert_eq!(logger.column_count(), 2);

        // Can still add rows with the same schema
        logger
            .log_row(1, vec![DataValue::Float(2.0), DataValue::Text("b".into())])
            .unwrap();
        assert_eq!(logger.row_count(), 1);
    }

    #[test]
    fn test_empty_logger_csv() {
        let mut logger = DataLogger::new("test");
        logger.add_column("a", ColumnType::Int).unwrap();
        logger.add_column("b", ColumnType::Float).unwrap();

        let csv = logger.to_csv();
        assert_eq!(csv, "timestamp,a,b\n");
    }

    #[test]
    fn test_single_row() {
        let mut logger = DataLogger::new("test");
        logger.add_column("x", ColumnType::Int).unwrap();

        logger.log_row(0, vec![DataValue::Int(99)]).unwrap();

        assert_eq!(logger.row_count(), 1);
        let row = logger.get_row(0).unwrap();
        assert_eq!(row.timestamp, 0);
        assert_eq!(row.values[0], DataValue::Int(99));
        assert!(logger.get_row(1).is_none());
    }

    #[test]
    fn test_single_column() {
        let mut logger = DataLogger::new("test");
        logger.add_column("only", ColumnType::Float).unwrap();

        logger.log_row(0, vec![DataValue::Float(3.14)]).unwrap();
        logger.log_row(1, vec![DataValue::Float(2.72)]).unwrap();

        let csv = logger.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3); // header + 2 rows
    }

    #[test]
    fn test_all_nulls() {
        let mut logger = DataLogger::new("test");
        logger.add_column("a", ColumnType::Int).unwrap();
        logger.add_column("b", ColumnType::Text).unwrap();
        logger.add_column("c", ColumnType::Bool).unwrap();

        logger
            .log_row(
                0,
                vec![DataValue::Null, DataValue::Null, DataValue::Null],
            )
            .unwrap();

        let csv = logger.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[1].ends_with(",,,"));

        // Stats for int column with all nulls
        let stats = logger.stats(0).unwrap();
        assert_eq!(stats.count, 0);
    }

    #[test]
    fn test_stats_with_nulls_mixed() {
        let mut logger = DataLogger::new("test");
        logger.add_column("v", ColumnType::Int).unwrap();

        logger.log_row(0, vec![DataValue::Int(10)]).unwrap();
        logger.log_row(1, vec![DataValue::Null]).unwrap();
        logger.log_row(2, vec![DataValue::Int(30)]).unwrap();

        let stats = logger.stats(0).unwrap();
        assert_eq!(stats.count, 2);
        assert_eq!(stats.min, 10.0);
        assert_eq!(stats.max, 30.0);
        assert!((stats.mean - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stats_out_of_bounds() {
        let logger = DataLogger::new("test");
        assert!(logger.stats(0).is_none());
        assert!(logger.stats(99).is_none());
    }

    #[test]
    fn test_get_row_out_of_bounds() {
        let logger = DataLogger::new("test");
        assert!(logger.get_row(0).is_none());
    }

    #[test]
    fn test_csv_bool_values() {
        let mut logger = DataLogger::new("test");
        logger.add_column("flag", ColumnType::Bool).unwrap();

        logger.log_row(0, vec![DataValue::Bool(true)]).unwrap();
        logger.log_row(1, vec![DataValue::Bool(false)]).unwrap();

        let csv = logger.to_csv();
        assert!(csv.contains(",true\n"));
        assert!(csv.contains(",false\n"));
    }

    // ── FFI tests ───────────────────────────────────────────────────

    #[test]
    fn test_ffi_create_destroy() {
        unsafe {
            let name = CString::new("ffi_test").unwrap();
            let logger = rs_data_logger_create(name.as_ptr());
            assert!(!logger.is_null());

            assert_eq!(rs_data_logger_row_count(logger), 0);
            assert_eq!(rs_data_logger_column_count(logger), 0);

            rs_data_logger_destroy(logger);
        }
    }

    #[test]
    fn test_ffi_null_pointer_safety() {
        unsafe {
            // Create with null name
            let logger = rs_data_logger_create(std::ptr::null());
            assert!(logger.is_null());

            // All operations on null logger should return error
            assert_eq!(
                rs_data_logger_add_column(std::ptr::null_mut(), std::ptr::null(), 0),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_begin_row(std::ptr::null_mut(), 0),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_set_int(std::ptr::null_mut(), 0, 0),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_set_float(std::ptr::null_mut(), 0, 0.0),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_set_text(std::ptr::null_mut(), 0, std::ptr::null()),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_set_bool(std::ptr::null_mut(), 0, 0),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_commit_row(std::ptr::null_mut()),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_row_count(std::ptr::null()),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_column_count(std::ptr::null()),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_data_logger_to_csv(std::ptr::null(), std::ptr::null_mut(), 0),
                ESP_ERR_INVALID_ARG
            );

            // Destroy null is safe (no-op)
            rs_data_logger_destroy(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_ffi_row_building() {
        unsafe {
            let name = CString::new("ffi_rows").unwrap();
            let logger = rs_data_logger_create(name.as_ptr());

            // Add columns
            let col_temp = CString::new("temperature").unwrap();
            let col_label = CString::new("label").unwrap();
            let col_active = CString::new("active").unwrap();

            let idx0 = rs_data_logger_add_column(logger, col_temp.as_ptr(), 1); // Float
            let idx1 = rs_data_logger_add_column(logger, col_label.as_ptr(), 2); // Text
            let idx2 = rs_data_logger_add_column(logger, col_active.as_ptr(), 3); // Bool

            assert_eq!(idx0, 0);
            assert_eq!(idx1, 1);
            assert_eq!(idx2, 2);
            assert_eq!(rs_data_logger_column_count(logger), 3);

            // Build a row
            assert_eq!(rs_data_logger_begin_row(logger, 1705321845), ESP_OK);
            assert_eq!(rs_data_logger_set_float(logger, 0, 23.5), ESP_OK);

            let text_val = CString::new("kitchen").unwrap();
            assert_eq!(rs_data_logger_set_text(logger, 1, text_val.as_ptr()), ESP_OK);
            assert_eq!(rs_data_logger_set_bool(logger, 2, 1), ESP_OK);
            assert_eq!(rs_data_logger_commit_row(logger), ESP_OK);

            assert_eq!(rs_data_logger_row_count(logger), 1);

            // Build another row with int set on a float column -> should fail
            assert_eq!(rs_data_logger_begin_row(logger, 1705321900), ESP_OK);
            assert_eq!(rs_data_logger_set_int(logger, 0, 42), ESP_ERR_INVALID_ARG); // Float col, not Int

            // Set correct type
            assert_eq!(rs_data_logger_set_float(logger, 0, 24.0), ESP_OK);
            let text_val2 = CString::new("bedroom").unwrap();
            assert_eq!(rs_data_logger_set_text(logger, 1, text_val2.as_ptr()), ESP_OK);
            assert_eq!(rs_data_logger_set_bool(logger, 2, 0), ESP_OK);
            assert_eq!(rs_data_logger_commit_row(logger), ESP_OK);

            assert_eq!(rs_data_logger_row_count(logger), 2);

            rs_data_logger_destroy(logger);
        }
    }

    #[test]
    fn test_ffi_csv_export() {
        unsafe {
            let name = CString::new("csv_test").unwrap();
            let logger = rs_data_logger_create(name.as_ptr());

            let col_name = CString::new("value").unwrap();
            rs_data_logger_add_column(logger, col_name.as_ptr(), 0); // Int

            assert_eq!(rs_data_logger_begin_row(logger, 0), ESP_OK);
            assert_eq!(rs_data_logger_set_int(logger, 0, 42), ESP_OK);
            assert_eq!(rs_data_logger_commit_row(logger), ESP_OK);

            // Export CSV
            let mut buf = vec![0u8; 1024];
            let len = rs_data_logger_to_csv(logger, buf.as_mut_ptr(), buf.len());
            assert!(len > 0);

            let csv = std::str::from_utf8(&buf[..len as usize]).unwrap();
            assert!(csv.starts_with("timestamp,value\n"));
            assert!(csv.contains("1970-01-01T00:00:00Z,42"));

            rs_data_logger_destroy(logger);
        }
    }

    #[test]
    fn test_ffi_buffer_too_small() {
        unsafe {
            let name = CString::new("small_buf").unwrap();
            let logger = rs_data_logger_create(name.as_ptr());

            let col_name = CString::new("val").unwrap();
            rs_data_logger_add_column(logger, col_name.as_ptr(), 0);

            assert_eq!(rs_data_logger_begin_row(logger, 0), ESP_OK);
            assert_eq!(rs_data_logger_set_int(logger, 0, 42), ESP_OK);
            assert_eq!(rs_data_logger_commit_row(logger), ESP_OK);

            // Buffer too small
            let mut buf = vec![0u8; 5];
            let result = rs_data_logger_to_csv(logger, buf.as_mut_ptr(), buf.len());
            assert_eq!(result, ESP_FAIL);

            rs_data_logger_destroy(logger);
        }
    }

    #[test]
    fn test_ffi_begin_row_twice_fails() {
        unsafe {
            let name = CString::new("double_begin").unwrap();
            let logger = rs_data_logger_create(name.as_ptr());

            let col_name = CString::new("v").unwrap();
            rs_data_logger_add_column(logger, col_name.as_ptr(), 0);

            assert_eq!(rs_data_logger_begin_row(logger, 0), ESP_OK);
            // Second begin without commit should fail
            assert_eq!(rs_data_logger_begin_row(logger, 1), ESP_FAIL);

            rs_data_logger_destroy(logger);
        }
    }

    #[test]
    fn test_ffi_commit_without_begin_fails() {
        unsafe {
            let name = CString::new("no_begin").unwrap();
            let logger = rs_data_logger_create(name.as_ptr());

            let col_name = CString::new("v").unwrap();
            rs_data_logger_add_column(logger, col_name.as_ptr(), 0);

            assert_eq!(rs_data_logger_commit_row(logger), ESP_FAIL);

            rs_data_logger_destroy(logger);
        }
    }

    #[test]
    fn test_ffi_set_column_out_of_bounds() {
        unsafe {
            let name = CString::new("oob").unwrap();
            let logger = rs_data_logger_create(name.as_ptr());

            let col_name = CString::new("v").unwrap();
            rs_data_logger_add_column(logger, col_name.as_ptr(), 0);

            assert_eq!(rs_data_logger_begin_row(logger, 0), ESP_OK);
            assert_eq!(rs_data_logger_set_int(logger, 5, 42), ESP_ERR_INVALID_ARG);
            assert_eq!(rs_data_logger_set_int(logger, -1, 42), ESP_ERR_INVALID_ARG);

            rs_data_logger_destroy(logger);
        }
    }

    #[test]
    fn test_ffi_invalid_column_type() {
        unsafe {
            let name = CString::new("bad_type").unwrap();
            let logger = rs_data_logger_create(name.as_ptr());

            let col_name = CString::new("v").unwrap();
            let result = rs_data_logger_add_column(logger, col_name.as_ptr(), 99);
            assert_eq!(result, ESP_ERR_INVALID_ARG);

            rs_data_logger_destroy(logger);
        }
    }

    #[test]
    fn test_unix_to_iso8601_epoch() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_unix_to_iso8601_known_date() {
        // 2024-01-15 12:30:45 UTC
        assert_eq!(unix_to_iso8601(1705321845), "2024-01-15T12:30:45Z");
    }

    #[test]
    fn test_unix_to_iso8601_y2k() {
        // 2000-01-01 00:00:00 UTC = 946684800
        assert_eq!(unix_to_iso8601(946684800), "2000-01-01T00:00:00Z");
    }

    #[test]
    fn test_csv_escape_plain() {
        assert_eq!(csv_escape("hello"), "hello");
    }

    #[test]
    fn test_csv_escape_comma() {
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
    }

    #[test]
    fn test_csv_escape_quote() {
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn test_multiple_rows_csv() {
        let mut logger = DataLogger::new("multi");
        logger.add_column("a", ColumnType::Int).unwrap();
        logger.add_column("b", ColumnType::Float).unwrap();
        logger.add_column("c", ColumnType::Text).unwrap();
        logger.add_column("d", ColumnType::Bool).unwrap();

        logger
            .log_row(
                100,
                vec![
                    DataValue::Int(1),
                    DataValue::Float(2.5),
                    DataValue::Text("hello".into()),
                    DataValue::Bool(true),
                ],
            )
            .unwrap();
        logger
            .log_row(
                200,
                vec![
                    DataValue::Int(2),
                    DataValue::Float(3.7),
                    DataValue::Text("world".into()),
                    DataValue::Bool(false),
                ],
            )
            .unwrap();

        let csv = logger.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "timestamp,a,b,c,d");
    }

    #[test]
    fn test_clear_then_add_columns_allowed() {
        let mut logger = DataLogger::new("test");
        logger.add_column("v", ColumnType::Int).unwrap();
        logger.log_row(0, vec![DataValue::Int(1)]).unwrap();

        logger.clear_rows();

        // Schema is locked even after clear (rows existed at some point) — actually,
        // clear_rows removes rows so we should be able to add columns again.
        // But the spec says "can't add columns after first row" and clear_rows says
        // "remove all rows (keep schema)". Since rows is now empty, add_column
        // checks self.rows.is_empty() which is true. So we CAN add columns.
        let result = logger.add_column("v2", ColumnType::Float);
        assert!(result.is_ok());
    }
}
