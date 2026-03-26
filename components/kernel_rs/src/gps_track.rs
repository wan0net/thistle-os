// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — GPS track logger & GPX export
//
// Records GPS positions into tracks, computes distance/bounds, and exports
// to GPX 1.1 XML for interoperability with mapping software.

use std::os::raw::c_char;
use std::ffi::CStr;

use crate::hal_registry::HalGpsPosition;

// ── ESP error codes ──────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_INVALID_ARG: i32 = 0x102;

// ── Constants ────────────────────────────────────────────────────────

const EARTH_RADIUS_KM: f64 = 6371.0;

// ── TrackPoint ───────────────────────────────────────────────────────

pub struct TrackPoint {
    pub position: HalGpsPosition,
    pub seq: u32,
    pub name: Option<String>,
}

// ── GpsTrack ─────────────────────────────────────────────────────────

pub struct GpsTrack {
    name: String,
    points: Vec<TrackPoint>,
    next_seq: u32,
}

impl GpsTrack {
    pub fn new(name: &str) -> GpsTrack {
        GpsTrack {
            name: name.to_string(),
            points: Vec::new(),
            next_seq: 0,
        }
    }

    pub fn add_point(&mut self, pos: HalGpsPosition) {
        let tp = TrackPoint {
            position: pos,
            seq: self.next_seq,
            name: None,
        };
        self.next_seq += 1;
        self.points.push(tp);
    }

    pub fn add_waypoint(&mut self, pos: HalGpsPosition, name: &str) {
        let tp = TrackPoint {
            position: pos,
            seq: self.next_seq,
            name: Some(name.to_string()),
        };
        self.next_seq += 1;
        self.points.push(tp);
    }

    pub fn points(&self) -> &[TrackPoint] {
        &self.points
    }

    pub fn waypoints(&self) -> Vec<&TrackPoint> {
        self.points.iter().filter(|p| p.name.is_some()).collect()
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.next_seq = 0;
    }

    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if self.points.is_empty() {
            return None;
        }
        let mut min_lat = f64::MAX;
        let mut min_lon = f64::MAX;
        let mut max_lat = f64::MIN;
        let mut max_lon = f64::MIN;
        for p in &self.points {
            let lat = p.position.latitude;
            let lon = p.position.longitude;
            if lat < min_lat { min_lat = lat; }
            if lat > max_lat { max_lat = lat; }
            if lon < min_lon { min_lon = lon; }
            if lon > max_lon { max_lon = lon; }
        }
        Some((min_lat, min_lon, max_lat, max_lon))
    }

    pub fn total_distance_km(&self) -> f64 {
        if self.points.len() < 2 {
            return 0.0;
        }
        let mut total = 0.0;
        for i in 1..self.points.len() {
            let a = &self.points[i - 1].position;
            let b = &self.points[i].position;
            total += haversine_distance_km(a.latitude, a.longitude, b.latitude, b.longitude);
        }
        total
    }

    pub fn elapsed_seconds(&self) -> u32 {
        if self.points.len() < 2 {
            return 0;
        }
        let first = self.points.first().unwrap().position.timestamp;
        let last = self.points.last().unwrap().position.timestamp;
        last.saturating_sub(first)
    }

    pub fn to_gpx(&self) -> String {
        let mut xml = String::with_capacity(4096);
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<gpx version=\"1.1\" creator=\"ThistleOS\" ");
        xml.push_str("xmlns=\"http://www.topografix.com/GPX/1/1\">\n");

        // Metadata
        xml.push_str("  <metadata>\n");
        xml.push_str("    <name>");
        xml_escape_into(&mut xml, &self.name);
        xml.push_str("</name>\n");
        if let Some(first) = self.points.first() {
            xml.push_str("    <time>");
            xml.push_str(&unix_to_iso8601(first.position.timestamp));
            xml.push_str("</time>\n");
        }
        xml.push_str("  </metadata>\n");

        // Waypoints
        for p in &self.points {
            if let Some(ref name) = p.name {
                xml.push_str(&format!(
                    "  <wpt lat=\"{:.7}\" lon=\"{:.7}\">\n",
                    p.position.latitude, p.position.longitude
                ));
                xml.push_str(&format!("    <ele>{:.1}</ele>\n", p.position.altitude_m));
                xml.push_str("    <time>");
                xml.push_str(&unix_to_iso8601(p.position.timestamp));
                xml.push_str("</time>\n");
                xml.push_str("    <name>");
                xml_escape_into(&mut xml, name);
                xml.push_str("</name>\n");
                xml.push_str("  </wpt>\n");
            }
        }

        // Track
        xml.push_str("  <trk>\n");
        xml.push_str("    <name>");
        xml_escape_into(&mut xml, &self.name);
        xml.push_str("</name>\n");
        xml.push_str("    <trkseg>\n");
        for p in &self.points {
            xml.push_str(&format!(
                "      <trkpt lat=\"{:.7}\" lon=\"{:.7}\">\n",
                p.position.latitude, p.position.longitude
            ));
            xml.push_str(&format!("        <ele>{:.1}</ele>\n", p.position.altitude_m));
            xml.push_str("        <time>");
            xml.push_str(&unix_to_iso8601(p.position.timestamp));
            xml.push_str("</time>\n");
            xml.push_str(&format!("        <speed>{:.2}</speed>\n", p.position.speed_kmh));
            xml.push_str(&format!("        <sat>{}</sat>\n", p.position.satellites));
            xml.push_str("      </trkpt>\n");
        }
        xml.push_str("    </trkseg>\n");
        xml.push_str("  </trk>\n");

        xml.push_str("</gpx>\n");
        xml
    }
}

// ── Haversine distance ───────────────────────────────────────────────

pub fn haversine_distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let lat1_r = lat1.to_radians();
    let lat2_r = lat2.to_radians();

    let a = (d_lat / 2.0).sin().powi(2)
        + lat1_r.cos() * lat2_r.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_KM * c
}

// ── Timestamp formatting ─────────────────────────────────────────────

fn unix_to_iso8601(ts: u32) -> String {
    let ts = ts as u64;
    // Seconds within day
    let secs_per_day: u64 = 86400;
    let mut days = ts / secs_per_day;
    let day_secs = ts % secs_per_day;
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;

    // Days since 1970-01-01 to (year, month, day)
    // Using a civil-from-days algorithm (Howard Hinnant)
    days += 719468; // shift epoch from 1970-01-01 to 0000-03-01
    let era = days / 146097;
    let doe = days - era * 146097; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month index [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hour, minute, second)
}

// ── XML escaping ─────────────────────────────────────────────────────

fn xml_escape_into(buf: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => buf.push_str("&amp;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '"' => buf.push_str("&quot;"),
            '\'' => buf.push_str("&apos;"),
            _ => buf.push(ch),
        }
    }
}

// ── C FFI exports ────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_gps_track_create(name: *const c_char) -> *mut GpsTrack {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let track = Box::new(GpsTrack::new(name_str));
    Box::into_raw(track)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_gps_track_destroy(track: *mut GpsTrack) {
    if !track.is_null() {
        let _ = Box::from_raw(track);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_gps_track_add_point(
    track: *mut GpsTrack,
    pos: *const HalGpsPosition,
) -> i32 {
    if track.is_null() || pos.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let track = &mut *track;
    let pos = *pos;
    track.add_point(pos);
    ESP_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_gps_track_add_waypoint(
    track: *mut GpsTrack,
    pos: *const HalGpsPosition,
    name: *const c_char,
) -> i32 {
    if track.is_null() || pos.is_null() || name.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let track = &mut *track;
    let pos = *pos;
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    track.add_waypoint(pos, name_str);
    ESP_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_gps_track_to_gpx(
    track: *const GpsTrack,
    buf: *mut u8,
    buf_len: usize,
) -> i32 {
    if track.is_null() || buf.is_null() || buf_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let track = &*track;
    let gpx = track.to_gpx();
    let bytes = gpx.as_bytes();
    if bytes.len() > buf_len {
        return ESP_FAIL;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
    bytes.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_gps_track_point_count(track: *const GpsTrack) -> i32 {
    if track.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let track = &*track;
    track.len() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_gps_track_distance_km(track: *const GpsTrack) -> f64 {
    if track.is_null() {
        return -1.0;
    }
    let track = &*track;
    track.total_distance_km()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn make_pos(lat: f64, lon: f64, alt: f32, ts: u32) -> HalGpsPosition {
        HalGpsPosition {
            latitude: lat,
            longitude: lon,
            altitude_m: alt,
            speed_kmh: 0.0,
            heading_deg: 0.0,
            satellites: 8,
            fix_valid: true,
            timestamp: ts,
        }
    }

    fn make_pos_full(
        lat: f64, lon: f64, alt: f32, speed: f32, heading: f32, sats: u8, ts: u32,
    ) -> HalGpsPosition {
        HalGpsPosition {
            latitude: lat,
            longitude: lon,
            altitude_m: alt,
            speed_kmh: speed,
            heading_deg: heading,
            satellites: sats,
            fix_valid: true,
            timestamp: ts,
        }
    }

    // ── Basic track operations ───────────────────────────────────────

    #[test]
    fn test_empty_track() {
        let t = GpsTrack::new("Test");
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert_eq!(t.points().len(), 0);
        assert!(t.waypoints().is_empty());
    }

    #[test]
    fn test_add_points_and_count() {
        let mut t = GpsTrack::new("Walk");
        t.add_point(make_pos(55.9533, -3.1883, 100.0, 1000));
        t.add_point(make_pos(55.9534, -3.1884, 101.0, 1010));
        t.add_point(make_pos(55.9535, -3.1885, 102.0, 1020));
        assert_eq!(t.len(), 3);
        assert!(!t.is_empty());
        assert_eq!(t.points()[0].seq, 0);
        assert_eq!(t.points()[1].seq, 1);
        assert_eq!(t.points()[2].seq, 2);
    }

    #[test]
    fn test_add_waypoints_and_filter() {
        let mut t = GpsTrack::new("Hike");
        t.add_point(make_pos(55.9, -3.1, 50.0, 100));
        t.add_waypoint(make_pos(55.95, -3.15, 120.0, 200), "Summit");
        t.add_point(make_pos(56.0, -3.2, 60.0, 300));
        t.add_waypoint(make_pos(56.05, -3.25, 80.0, 400), "Loch");

        assert_eq!(t.len(), 4);
        let wps = t.waypoints();
        assert_eq!(wps.len(), 2);
        assert_eq!(wps[0].name.as_deref(), Some("Summit"));
        assert_eq!(wps[1].name.as_deref(), Some("Loch"));
    }

    #[test]
    fn test_clear() {
        let mut t = GpsTrack::new("Temp");
        t.add_point(make_pos(55.0, -3.0, 0.0, 100));
        t.add_point(make_pos(55.1, -3.1, 0.0, 200));
        assert_eq!(t.len(), 2);
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        // Sequence resets after clear
        t.add_point(make_pos(55.0, -3.0, 0.0, 300));
        assert_eq!(t.points()[0].seq, 0);
    }

    // ── Bounds ───────────────────────────────────────────────────────

    #[test]
    fn test_bounds_empty() {
        let t = GpsTrack::new("Empty");
        assert!(t.bounds().is_none());
    }

    #[test]
    fn test_bounds_single_point() {
        let mut t = GpsTrack::new("Single");
        t.add_point(make_pos(55.9533, -3.1883, 100.0, 1000));
        let b = t.bounds().unwrap();
        assert_eq!(b.0, 55.9533);
        assert_eq!(b.1, -3.1883);
        assert_eq!(b.2, 55.9533);
        assert_eq!(b.3, -3.1883);
    }

    #[test]
    fn test_bounds_multiple_points() {
        let mut t = GpsTrack::new("Route");
        t.add_point(make_pos(55.0, -3.0, 0.0, 100));
        t.add_point(make_pos(56.0, -4.0, 0.0, 200));
        t.add_point(make_pos(55.5, -3.5, 0.0, 300));
        let (min_lat, min_lon, max_lat, max_lon) = t.bounds().unwrap();
        assert!((min_lat - 55.0).abs() < 1e-10);
        assert!((min_lon - (-4.0)).abs() < 1e-10);
        assert!((max_lat - 56.0).abs() < 1e-10);
        assert!((max_lon - (-3.0)).abs() < 1e-10);
    }

    // ── Haversine distance ───────────────────────────────────────────

    #[test]
    fn test_haversine_zero_distance() {
        let d = haversine_distance_km(55.0, -3.0, 55.0, -3.0);
        assert!(d.abs() < 1e-10);
    }

    #[test]
    fn test_haversine_edinburgh_to_glasgow() {
        // Edinburgh: 55.9533, -3.1883
        // Glasgow:   55.8642, -4.2518
        let d = haversine_distance_km(55.9533, -3.1883, 55.8642, -4.2518);
        // Known distance is roughly 67 km
        assert!(d > 60.0, "Distance should be >60km, got {}", d);
        assert!(d < 75.0, "Distance should be <75km, got {}", d);
    }

    #[test]
    fn test_haversine_symmetric() {
        let d1 = haversine_distance_km(55.0, -3.0, 56.0, -4.0);
        let d2 = haversine_distance_km(56.0, -4.0, 55.0, -3.0);
        assert!((d1 - d2).abs() < 1e-10);
    }

    #[test]
    fn test_haversine_equator() {
        // One degree of longitude at the equator ~ 111.32 km
        let d = haversine_distance_km(0.0, 0.0, 0.0, 1.0);
        assert!(d > 110.0 && d < 112.0, "Got {}", d);
    }

    // ── Total distance along track ──────────────────────────────────

    #[test]
    fn test_total_distance_empty() {
        let t = GpsTrack::new("Empty");
        assert!(t.total_distance_km().abs() < 1e-10);
    }

    #[test]
    fn test_total_distance_single_point() {
        let mut t = GpsTrack::new("One");
        t.add_point(make_pos(55.0, -3.0, 0.0, 100));
        assert!(t.total_distance_km().abs() < 1e-10);
    }

    #[test]
    fn test_total_distance_accumulation() {
        let mut t = GpsTrack::new("Trip");
        // Three points forming a path
        t.add_point(make_pos(55.0, -3.0, 0.0, 100));
        t.add_point(make_pos(55.1, -3.0, 0.0, 200));
        t.add_point(make_pos(55.1, -3.1, 0.0, 300));
        let d = t.total_distance_km();
        // Each leg should be roughly 11 km, total ~ 22 km
        assert!(d > 15.0, "Distance should be >15km, got {}", d);
        assert!(d < 25.0, "Distance should be <25km, got {}", d);
    }

    #[test]
    fn test_same_point_repeated() {
        let mut t = GpsTrack::new("Static");
        for i in 0..5 {
            t.add_point(make_pos(55.0, -3.0, 0.0, i * 10));
        }
        assert!(t.total_distance_km().abs() < 1e-10);
    }

    // ── Elapsed seconds ─────────────────────────────────────────────

    #[test]
    fn test_elapsed_empty() {
        let t = GpsTrack::new("Empty");
        assert_eq!(t.elapsed_seconds(), 0);
    }

    #[test]
    fn test_elapsed_single_point() {
        let mut t = GpsTrack::new("One");
        t.add_point(make_pos(55.0, -3.0, 0.0, 1000));
        assert_eq!(t.elapsed_seconds(), 0);
    }

    #[test]
    fn test_elapsed_multiple_points() {
        let mut t = GpsTrack::new("Walk");
        t.add_point(make_pos(55.0, -3.0, 0.0, 1000));
        t.add_point(make_pos(55.1, -3.1, 0.0, 1500));
        t.add_point(make_pos(55.2, -3.2, 0.0, 2000));
        assert_eq!(t.elapsed_seconds(), 1000);
    }

    // ── ISO 8601 timestamp ──────────────────────────────────────────

    #[test]
    fn test_unix_epoch() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_known_timestamp() {
        // 2024-01-15T12:30:00Z = 1705321800
        assert_eq!(unix_to_iso8601(1705321800), "2024-01-15T12:30:00Z");
    }

    #[test]
    fn test_timestamp_y2k() {
        // 2000-01-01T00:00:00Z = 946684800
        assert_eq!(unix_to_iso8601(946684800), "2000-01-01T00:00:00Z");
    }

    // ── GPX output ──────────────────────────────────────────────────

    #[test]
    fn test_gpx_empty_track() {
        let t = GpsTrack::new("Empty Track");
        let gpx = t.to_gpx();
        assert!(gpx.contains("<?xml version=\"1.0\""));
        assert!(gpx.contains("<gpx version=\"1.1\""));
        assert!(gpx.contains("<name>Empty Track</name>"));
        assert!(gpx.contains("<trkseg>"));
        assert!(gpx.contains("</gpx>"));
        // Should not contain any trkpt elements
        assert!(!gpx.contains("<trkpt"));
    }

    #[test]
    fn test_gpx_valid_structure() {
        let mut t = GpsTrack::new("Test Route");
        t.add_point(make_pos_full(55.9533, -3.1883, 120.5, 5.2, 90.0, 10, 1705321800));
        let gpx = t.to_gpx();
        assert!(gpx.contains("<metadata>"));
        assert!(gpx.contains("</metadata>"));
        assert!(gpx.contains("<trk>"));
        assert!(gpx.contains("</trk>"));
        assert!(gpx.contains("<trkseg>"));
        assert!(gpx.contains("</trkseg>"));
        assert!(gpx.contains("<trkpt"));
        assert!(gpx.contains("</trkpt>"));
    }

    #[test]
    fn test_gpx_lat_lon_values() {
        let mut t = GpsTrack::new("Coords");
        t.add_point(make_pos(55.9533000, -3.1883000, 100.0, 1000));
        let gpx = t.to_gpx();
        assert!(gpx.contains("lat=\"55.9533000\""), "GPX: {}", gpx);
        assert!(gpx.contains("lon=\"-3.1883000\""), "GPX: {}", gpx);
    }

    #[test]
    fn test_gpx_elevation_and_speed() {
        let mut t = GpsTrack::new("Metrics");
        t.add_point(make_pos_full(55.0, -3.0, 250.5, 12.34, 0.0, 7, 1000));
        let gpx = t.to_gpx();
        assert!(gpx.contains("<ele>250.5</ele>"), "GPX: {}", gpx);
        assert!(gpx.contains("<speed>12.34</speed>"), "GPX: {}", gpx);
        assert!(gpx.contains("<sat>7</sat>"), "GPX: {}", gpx);
    }

    #[test]
    fn test_gpx_timestamp() {
        let mut t = GpsTrack::new("Timed");
        t.add_point(make_pos(55.0, -3.0, 0.0, 1705321800));
        let gpx = t.to_gpx();
        assert!(gpx.contains("<time>2024-01-15T12:30:00Z</time>"), "GPX: {}", gpx);
    }

    #[test]
    fn test_gpx_waypoint_names() {
        let mut t = GpsTrack::new("Waypoints");
        t.add_waypoint(make_pos(55.95, -3.19, 200.0, 1000), "Castle");
        t.add_waypoint(make_pos(55.96, -3.20, 150.0, 2000), "Bridge");
        let gpx = t.to_gpx();
        assert!(gpx.contains("<wpt lat=\"55.9500000\" lon=\"-3.1900000\">"), "GPX: {}", gpx);
        assert!(gpx.contains("<name>Castle</name>"), "GPX: {}", gpx);
        assert!(gpx.contains("<name>Bridge</name>"), "GPX: {}", gpx);
        // Waypoints also appear as trkpt
        assert!(gpx.contains("<trkpt lat=\"55.9500000\" lon=\"-3.1900000\">"), "GPX: {}", gpx);
    }

    #[test]
    fn test_gpx_xml_escaping() {
        let mut t = GpsTrack::new("Trail & <Route>");
        t.add_waypoint(make_pos(55.0, -3.0, 0.0, 100), "Ben \"Big\" & <Small>");
        let gpx = t.to_gpx();
        assert!(gpx.contains("Trail &amp; &lt;Route&gt;"), "GPX: {}", gpx);
        assert!(gpx.contains("Ben &quot;Big&quot; &amp; &lt;Small&gt;"), "GPX: {}", gpx);
    }

    #[test]
    fn test_gpx_metadata_time() {
        let mut t = GpsTrack::new("Meta");
        t.add_point(make_pos(55.0, -3.0, 0.0, 946684800));
        let gpx = t.to_gpx();
        // Metadata time should be the first point's timestamp
        assert!(gpx.contains("<metadata>"));
        assert!(gpx.contains("<time>2000-01-01T00:00:00Z</time>"));
    }

    // ── FFI tests ───────────────────────────────────────────────────

    #[test]
    fn test_ffi_create_and_destroy() {
        let name = CString::new("FFI Track").unwrap();
        unsafe {
            let track = rs_gps_track_create(name.as_ptr());
            assert!(!track.is_null());
            assert_eq!(rs_gps_track_point_count(track), 0);
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_create_null_name() {
        unsafe {
            let track = rs_gps_track_create(std::ptr::null());
            assert!(track.is_null());
        }
    }

    #[test]
    fn test_ffi_add_point() {
        let name = CString::new("FFI Points").unwrap();
        unsafe {
            let track = rs_gps_track_create(name.as_ptr());
            let pos = make_pos(55.0, -3.0, 0.0, 1000);
            let rc = rs_gps_track_add_point(track, &pos);
            assert_eq!(rc, ESP_OK);
            assert_eq!(rs_gps_track_point_count(track), 1);
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_add_point_null_track() {
        unsafe {
            let pos = make_pos(55.0, -3.0, 0.0, 1000);
            let rc = rs_gps_track_add_point(std::ptr::null_mut(), &pos);
            assert_eq!(rc, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_ffi_add_point_null_pos() {
        let name = CString::new("NullPos").unwrap();
        unsafe {
            let track = rs_gps_track_create(name.as_ptr());
            let rc = rs_gps_track_add_point(track, std::ptr::null());
            assert_eq!(rc, ESP_ERR_INVALID_ARG);
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_add_waypoint() {
        let track_name = CString::new("WP Track").unwrap();
        let wp_name = CString::new("Peak").unwrap();
        unsafe {
            let track = rs_gps_track_create(track_name.as_ptr());
            let pos = make_pos(55.5, -3.5, 800.0, 2000);
            let rc = rs_gps_track_add_waypoint(track, &pos, wp_name.as_ptr());
            assert_eq!(rc, ESP_OK);
            assert_eq!(rs_gps_track_point_count(track), 1);
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_add_waypoint_null_name() {
        let track_name = CString::new("WP Track").unwrap();
        unsafe {
            let track = rs_gps_track_create(track_name.as_ptr());
            let pos = make_pos(55.5, -3.5, 800.0, 2000);
            let rc = rs_gps_track_add_waypoint(track, &pos, std::ptr::null());
            assert_eq!(rc, ESP_ERR_INVALID_ARG);
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_to_gpx() {
        let name = CString::new("GPX Export").unwrap();
        unsafe {
            let track = rs_gps_track_create(name.as_ptr());
            let pos = make_pos(55.0, -3.0, 100.0, 1000);
            rs_gps_track_add_point(track, &pos);

            let mut buf = vec![0u8; 8192];
            let rc = rs_gps_track_to_gpx(track, buf.as_mut_ptr(), buf.len());
            assert!(rc > 0, "Expected positive byte count, got {}", rc);
            let gpx = std::str::from_utf8(&buf[..rc as usize]).unwrap();
            assert!(gpx.contains("<gpx"));
            assert!(gpx.contains("GPX Export"));
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_to_gpx_buffer_too_small() {
        let name = CString::new("Big").unwrap();
        unsafe {
            let track = rs_gps_track_create(name.as_ptr());
            let pos = make_pos(55.0, -3.0, 0.0, 1000);
            rs_gps_track_add_point(track, &pos);

            let mut buf = [0u8; 10]; // Way too small
            let rc = rs_gps_track_to_gpx(track, buf.as_mut_ptr(), buf.len());
            assert_eq!(rc, ESP_FAIL);
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_to_gpx_null_track() {
        unsafe {
            let mut buf = [0u8; 1024];
            let rc = rs_gps_track_to_gpx(std::ptr::null(), buf.as_mut_ptr(), buf.len());
            assert_eq!(rc, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_ffi_to_gpx_null_buf() {
        let name = CString::new("NB").unwrap();
        unsafe {
            let track = rs_gps_track_create(name.as_ptr());
            let rc = rs_gps_track_to_gpx(track, std::ptr::null_mut(), 1024);
            assert_eq!(rc, ESP_ERR_INVALID_ARG);
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_point_count_null() {
        unsafe {
            let rc = rs_gps_track_point_count(std::ptr::null());
            assert_eq!(rc, ESP_ERR_INVALID_ARG);
        }
    }

    #[test]
    fn test_ffi_distance_null() {
        unsafe {
            let d = rs_gps_track_distance_km(std::ptr::null());
            assert!(d < 0.0);
        }
    }

    #[test]
    fn test_ffi_distance_valid() {
        let name = CString::new("Dist").unwrap();
        unsafe {
            let track = rs_gps_track_create(name.as_ptr());
            // Edinburgh to Glasgow
            let p1 = make_pos(55.9533, -3.1883, 0.0, 100);
            let p2 = make_pos(55.8642, -4.2518, 0.0, 200);
            rs_gps_track_add_point(track, &p1);
            rs_gps_track_add_point(track, &p2);
            let d = rs_gps_track_distance_km(track);
            assert!(d > 60.0 && d < 75.0, "Distance: {}", d);
            rs_gps_track_destroy(track);
        }
    }

    #[test]
    fn test_ffi_destroy_null_is_safe() {
        unsafe {
            rs_gps_track_destroy(std::ptr::null_mut());
            // Should not crash
        }
    }
}
